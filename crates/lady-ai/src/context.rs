//! Context building, token budgeting, and the secret-redaction pass (PH5-005).
//!
//! This module is pure and git-free: it operates on already-gathered facts
//! (diffs as [`lady_proto::FileDiff`], branch name, recent commit messages) so
//! it can be unit-tested in isolation. The host app gathers the facts via
//! `lady-git` and calls these builders.
//!
//! **Redaction (ADR-0009) is best-effort, not a guarantee.** [`redact`] strips
//! obvious credentials (regex) and high-entropy tokens before any *remote*
//! send; it reduces accidental leakage but does not make sending code safe by
//! itself. Local Ollama may skip redaction; the budgeting still applies.

use std::sync::OnceLock;

use lady_proto::{FileDiff, LineKind};
use regex::Regex;

/// A token + byte budget for a payload sent to a model.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    /// Soft cap on prompt tokens (leaves room for the response).
    pub max_tokens: usize,
    /// Hard cap on payload bytes, regardless of token count.
    pub max_bytes: usize,
}

impl Budget {
    /// A budget derived from a provider context window: reserve ~30% for the
    /// response, and cap bytes at ~4 bytes/token.
    pub fn for_context_window(window: usize) -> Self {
        let max_tokens = (window.saturating_mul(7) / 10).max(512);
        Budget {
            max_tokens,
            max_bytes: max_tokens.saturating_mul(4),
        }
    }
}

/// Commit-message style primed from recent history.
#[derive(Clone, Debug, Default)]
pub struct CommitStyle {
    /// Whether the repo uses Conventional Commits.
    pub conventional: bool,
    /// Recent subject lines (most recent first), for tone priming.
    pub recent: Vec<String>,
}

/// Detect Conventional Commits usage from recent subject lines: a simple
/// majority of `type(scope)?: subject` shaped messages.
pub fn detect_conventional(recent: &[String]) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([^)]*\))?!?: ",
        )
        .expect("regex pattern must be valid (programming error if this panics)")
    });
    if recent.is_empty() {
        return false;
    }
    let hits = recent.iter().filter(|m| re.is_match(m)).count();
    hits * 2 >= recent.len()
}

/// Build [`CommitStyle`] from recent subject lines.
pub fn commit_style(recent: &[String]) -> CommitStyle {
    CommitStyle {
        conventional: detect_conventional(recent),
        recent: recent.to_vec(),
    }
}

// ── Token counting ───────────────────────────────────────────────────────────

/// Count tokens with tiktoken's `cl100k_base` (a good cross-provider proxy).
pub fn count_tokens(text: &str) -> usize {
    static BPE: OnceLock<tiktoken_rs::CoreBPE> = OnceLock::new();
    let bpe = BPE.get_or_init(|| {
        tiktoken_rs::cl100k_base()
            .expect("tiktoken cl100k_base must load (programming error if this panics)")
    });
    bpe.encode_ordinary(text).len()
}

// ── Diff rendering + budgeting ─────────────────────────────────────────────────

/// Render one file's diff to unified-ish text (header + hunks).
fn render_file(f: &FileDiff) -> String {
    let mut out = String::new();
    let header = match &f.old_path {
        Some(old) if old != &f.path => format!("diff --- {} +++ {}\n", old, f.path),
        _ => format!("diff --- a/{0} +++ b/{0}\n", f.path),
    };
    out.push_str(&header);
    for h in &f.hunks {
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            h.old_start, h.old_lines, h.new_start, h.new_lines
        ));
        for line in &h.lines {
            let sigil = match line.kind {
                LineKind::Added => '+',
                LineKind::Deleted => '-',
                LineKind::Context => ' ',
            };
            out.push(sigil);
            out.push_str(&line.content);
            if !line.content.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

/// A diff rendered to text and fit to `budget`. Files are taken in order; once
/// the budget is reached the remainder is dropped and a deterministic note is
/// appended naming how many files/hunks were omitted. (v1 fits by truncation;
/// full map-reduce summarization is a documented follow-up.)
pub fn budget_diff(files: &[FileDiff], budget: Budget) -> String {
    let mut out = String::new();
    let mut used_tokens = 0usize;
    let mut included = 0usize;
    let mut omitted_hunks = 0usize;

    for f in files {
        let rendered = render_file(f);
        let toks = count_tokens(&rendered);
        let within_bytes = out.len() + rendered.len() <= budget.max_bytes;
        if used_tokens + toks <= budget.max_tokens && within_bytes {
            out.push_str(&rendered);
            used_tokens += toks;
            included += 1;
        } else {
            omitted_hunks += f.hunks.len();
        }
    }

    let omitted_files = files.len() - included;
    if omitted_files > 0 {
        out.push_str(&format!(
            "\n[... {omitted_files} file(s) / {omitted_hunks} hunk(s) omitted to fit the model budget ...]\n"
        ));
    }
    out
}

// ── Secret redaction (ADR-0009) ────────────────────────────────────────────────

/// The replacement token used for redacted secrets.
pub const REDACTION: &str = "[REDACTED]";

fn secret_patterns() -> &'static [Regex] {
    static PATS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATS.get_or_init(|| {
        [
            // PEM private key blocks.
            r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
            // AWS access key id.
            r"AKIA[0-9A-Z]{16}",
            // GitHub / GitLab tokens.
            r"gh[pousr]_[A-Za-z0-9]{20,}",
            r"glpat-[A-Za-z0-9_-]{20,}",
            // Slack tokens.
            r"xox[baprs]-[A-Za-z0-9-]{10,}",
            // OpenAI-style keys.
            r"sk-[A-Za-z0-9_-]{20,}",
            // Google API key.
            r"AIza[0-9A-Za-z_-]{35}",
            // key=value / key: value credentials (redact the value).
            r#"(?i)(password|passwd|secret|api[_-]?key|access[_-]?token|auth[_-]?token|bearer)([\"']?\s*[:=]\s*[\"']?)[^\s\"']{6,}"#,
        ]
        .iter()
        .map(|p| Regex::new(p).expect("secret regex pattern must be valid (programming error if this panics)"))
        .collect()
    })
}

/// Shannon entropy (bits/char) of `s`.
fn entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut counts = std::collections::HashMap::new();
    for c in s.chars() {
        *counts.entry(c).or_insert(0u32) += 1;
    }
    let len = s.chars().count() as f64;
    counts
        .values()
        .map(|&n| {
            let p = n as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// Whether a token looks like a high-entropy secret (long base64/hex blob).
fn looks_secret(tok: &str) -> bool {
    if tok.len() < 24 {
        return false;
    }
    let charset_ok = tok
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-'));
    charset_ok && entropy(tok) > 4.0
}

/// Redact obvious secrets from `text` before a remote send (ADR-0009).
/// Returns the redacted text and the number of redactions applied.
///
/// Best-effort only — pattern + entropy scanning, not a guarantee.
pub fn redact(text: &str) -> (String, usize) {
    let mut count = 0usize;
    let mut out = text.to_string();

    // 1) Pattern-based: replace each match with the redaction marker. For the
    //    key=value pattern, preserve the key + separator and redact the value.
    let pats = secret_patterns();
    for (i, re) in pats.iter().enumerate() {
        let is_kv = i == pats.len() - 1;
        out = re
            .replace_all(&out, |caps: &regex::Captures| {
                count += 1;
                if is_kv {
                    format!("{}{}{}", &caps[1], &caps[2], REDACTION)
                } else {
                    REDACTION.to_string()
                }
            })
            .into_owned();
    }

    // 2) Entropy-based: scan remaining whitespace/quote-delimited tokens.
    let mut rebuilt = String::with_capacity(out.len());
    let mut tok = String::new();
    let flush = |rebuilt: &mut String, tok: &mut String, count: &mut usize| {
        if !tok.is_empty() {
            if tok != REDACTION && looks_secret(tok) {
                rebuilt.push_str(REDACTION);
                *count += 1;
            } else {
                rebuilt.push_str(tok);
            }
            tok.clear();
        }
    };
    for c in out.chars() {
        if c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '(' | ')' | ',' | ';') {
            flush(&mut rebuilt, &mut tok, &mut count);
            rebuilt.push(c);
        } else {
            tok.push(c);
        }
    }
    flush(&mut rebuilt, &mut tok, &mut count);

    (rebuilt, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lady_proto::{DiffHunk, DiffLine, FileDiffKind};

    fn line(kind: LineKind, content: &str) -> DiffLine {
        DiffLine {
            kind,
            content: content.to_string(),
        }
    }

    fn file(path: &str, n: usize) -> FileDiff {
        let lines: Vec<DiffLine> = (0..n)
            .map(|i| line(LineKind::Added, &format!("let x{i} = {i};")))
            .collect();
        FileDiff {
            path: path.to_string(),
            old_path: None,
            kind: FileDiffKind::Modified,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 0,
                new_start: 1,
                new_lines: n as u32,
                lines,
            }],
            has_null_bytes: false,
            old_image_b64: None,
            new_image_b64: None,
        }
    }

    #[test]
    fn detects_conventional_commits() {
        assert!(detect_conventional(&[
            "feat: add x".into(),
            "fix(ui): y".into(),
            "random".into(),
        ]));
        assert!(!detect_conventional(&[
            "update stuff".into(),
            "more stuff".into(),
        ]));
    }

    #[test]
    fn redaction_strips_planted_secrets() {
        let text = "\
const AWS = \"AKIAIOSFODNN7EXAMPLE\";
let gh = ghp_abcdefghijklmnopqrstuvwxyz0123456789;
password = hunter2secret
api_key: \"sk-abcdefghijklmnopqrstuvwxyz123456\"
let normal = 42;
";
        let (red, n) = redact(text);
        assert!(n >= 4, "expected several redactions, got {n}");
        assert!(
            !red.contains("AKIAIOSFODNN7EXAMPLE"),
            "aws key leaked: {red}"
        );
        assert!(!red.contains("ghp_abcdefghij"), "gh token leaked: {red}");
        assert!(!red.contains("hunter2secret"), "password leaked: {red}");
        assert!(!red.contains("sk-abcdefghij"), "openai key leaked: {red}");
        // The key name survives so the message stays readable.
        assert!(red.contains("password"));
        // Innocuous code is preserved.
        assert!(red.contains("let normal = 42;"));
    }

    #[test]
    fn redaction_catches_high_entropy_blob() {
        let blob = "Zm9vYmFyMTIzNDU2Nzg5MEFCQ0RFRkdISUpLTE1OT1A=";
        let text = format!("token {blob} end");
        let (red, n) = redact(&text);
        assert!(n >= 1, "entropy blob not redacted");
        assert!(!red.contains(blob));
    }

    #[test]
    fn over_budget_diff_is_truncated_deterministically() {
        let files = vec![file("a.rs", 50), file("b.rs", 50), file("c.rs", 50)];
        let big = Budget {
            max_tokens: 1_000_000,
            max_bytes: 10_000_000,
        };
        // Size one file, then budget for ~1.5 of them so exactly one fits.
        let one = count_tokens(&budget_diff(&files[..1], big));
        let tiny = Budget {
            max_tokens: one + one / 2,
            max_bytes: 100_000,
        };
        let out1 = budget_diff(&files, tiny);
        let out2 = budget_diff(&files, tiny);
        assert_eq!(out1, out2, "budgeting must be deterministic");
        assert!(out1.contains("omitted to fit the model budget"));
        // First file is included; later ones omitted.
        assert!(out1.contains("a.rs"));
        assert!(!out1.contains("c.rs"));

        // A generous budget includes everything, no omission note.
        let full = budget_diff(&files, big);
        assert!(full.contains("c.rs"));
        assert!(!full.contains("omitted to fit"));
    }
}
