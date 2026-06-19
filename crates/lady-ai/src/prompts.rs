//! Prompt assembly per task (PH5-006..010). Pure functions returning
//! `(system, user_prompt)` pairs so they can be unit-tested without a provider.
//!
//! These take already-budgeted, already-redacted text from [`crate::context`].

use crate::context::CommitStyle;

/// What [`crate::AiTask::Explain`] targets, for tailored phrasing.
#[derive(Clone, Copy, Debug)]
pub enum ExplainTarget {
    /// A single commit.
    Commit,
    /// A user-selected set of commits.
    Commits,
    /// A branch / commit range.
    BranchRange,
    /// A stash entry.
    Stash,
    /// The current working changes.
    WorkingChanges,
    /// A specific set of changes (a file's diff, or a single hunk).
    Changes,
}

impl ExplainTarget {
    fn noun(self) -> &'static str {
        match self {
            ExplainTarget::Commit => "commit",
            ExplainTarget::Commits => "set of commits",
            ExplainTarget::BranchRange => "range of commits",
            ExplainTarget::Stash => "stashed changes",
            ExplainTarget::WorkingChanges => "working-tree changes",
            ExplainTarget::Changes => "code changes",
        }
    }
}

/// Commit message prompt honoring the detected convention (PH5-006).
pub fn commit_message(diff_text: &str, style: &CommitStyle) -> (String, String) {
    let convention = if style.conventional {
        "The repository uses Conventional Commits. Produce a `type(scope): subject` \
         subject line (≤72 chars) followed by an optional body explaining why."
    } else {
        "Match the style of the recent commit messages below. Keep the subject ≤72 chars, \
         then an optional body explaining why."
    };
    let system = format!(
        "You are a precise Git assistant. Write a single commit message for the staged diff. \
         {convention} Output ONLY the commit message — no markdown fences, no preamble."
    );
    let mut prompt = String::new();
    if !style.recent.is_empty() {
        prompt.push_str("Recent commit subjects (for style):\n");
        for m in style.recent.iter().take(10) {
            prompt.push_str(&format!("- {m}\n"));
        }
        prompt.push('\n');
    }
    prompt.push_str("Staged diff:\n");
    prompt.push_str(diff_text);
    (system, prompt)
}

/// Commit Composer prompt: ask for a strict JSON plan (PH5-007). Hunks are
/// referenced by the stable ids the host assigns (`path:index`).
pub fn split_commits(diff_text: &str, hunk_ids: &[String]) -> (String, String) {
    let system = "You are a Git assistant that organizes a messy working tree into logical \
         commits. Group the provided hunks into a small number of cohesive commits. \
         Respond with STRICT JSON only, no markdown, of the shape: \
         {\"commits\":[{\"message\":\"<commit message>\",\"hunk_ids\":[\"<id>\",...]}]}. \
         Every hunk id must appear in exactly one commit; use only the ids provided."
        .to_string();
    let prompt = format!(
        "Available hunk ids:\n{}\n\nWorking diff:\n{}",
        hunk_ids
            .iter()
            .map(|h| format!("- {h}"))
            .collect::<Vec<_>>()
            .join("\n"),
        diff_text
    );
    (system, prompt)
}

/// Explain prompt for a target (PH5-008). `content` is the diff/log text.
pub fn explain(target: ExplainTarget, content: &str) -> (String, String) {
    let system = format!(
        "You are a senior engineer. Explain the following {} in clear, plain English: \
         what changed and why it matters. Be concise. Use short paragraphs or bullets.",
        target.noun()
    );
    (system, content.to_string())
}

/// Conflict-resolution prompt for one region (PH5-009). Review-gated: the model
/// proposes; the user confirms in the resolver.
pub fn resolve_conflict(path: &str, base: &str, ours: &str, theirs: &str) -> (String, String) {
    let system = "You resolve a single Git merge conflict region. Combine the intent of both \
         sides correctly. Output ONLY the resolved code for the region — no conflict \
         markers, no markdown fences, no commentary."
        .to_string();
    let prompt = format!(
        "File: {path}\n\n=== BASE (common ancestor) ===\n{base}\n\
         === OURS ===\n{ours}\n=== THEIRS ===\n{theirs}\n\nResolved region:"
    );
    (system, prompt)
}

/// PR/MR title prompt over a branch's commit subjects (PH5-010).
pub fn pr_title(commit_subjects: &[String], diff_text: &str) -> (String, String) {
    let system = "Write a concise pull/merge request title (≤72 chars) summarizing the change. \
         Output ONLY the title."
        .to_string();
    let prompt = format!(
        "Commits:\n{}\n\nDiff:\n{}",
        commit_subjects.join("\n"),
        diff_text
    );
    (system, prompt)
}

/// PR/MR description prompt (PH5-010).
pub fn pr_description(commit_subjects: &[String], diff_text: &str) -> (String, String) {
    let system = "Write a clear pull/merge request description in markdown: a short summary, \
         a bulleted list of notable changes, and any testing notes. Output ONLY the body."
        .to_string();
    let prompt = format!(
        "Commits:\n{}\n\nDiff:\n{}",
        commit_subjects.join("\n"),
        diff_text
    );
    (system, prompt)
}

/// Changelog prompt grouping a range by Conventional-Commit type (PH5-010).
pub fn changelog(commit_subjects: &[String]) -> (String, String) {
    let system = "Produce a changelog in markdown grouping the commits by Conventional Commit \
         type (Features, Fixes, Performance, Refactors, Docs, Other) under `###` headings. \
         Omit empty groups. Output ONLY the changelog."
        .to_string();
    let prompt = format!("Commits:\n{}", commit_subjects.join("\n"));
    (system, prompt)
}

/// Stash-note prompt summarizing working changes (PH5-010).
pub fn stash_note(diff_text: &str) -> (String, String) {
    let system = "Summarize these uncommitted working changes into a single short stash \
         description (≤60 chars). Output ONLY the description."
        .to_string();
    (system, format!("Working diff:\n{diff_text}"))
}

/// A validated Commit Composer plan (PH5-007).
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct CommitPlan {
    /// The proposed commits, in apply order.
    pub commits: Vec<PlannedCommit>,
}

/// One proposed commit in a [`CommitPlan`].
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct PlannedCommit {
    /// The commit message.
    pub message: String,
    /// The hunk ids assigned to this commit.
    pub hunk_ids: Vec<String>,
}

/// Parse + validate a model's Commit Composer plan against the known hunk ids.
/// Tolerates markdown code fences around the JSON. Rejects unknown ids and
/// requires every known id to be covered exactly once.
pub fn parse_commit_plan(raw: &str, known_ids: &[String]) -> crate::Result<CommitPlan> {
    let json = strip_code_fence(raw);
    let plan: CommitPlan = serde_json::from_str(json)
        .map_err(|e| crate::Error::BadOutput(format!("plan JSON: {e}")))?;
    let known: std::collections::HashSet<&String> = known_ids.iter().collect();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for c in &plan.commits {
        if c.message.trim().is_empty() {
            return Err(crate::Error::BadOutput("empty commit message".into()));
        }
        for id in &c.hunk_ids {
            if !known.contains(id) {
                return Err(crate::Error::BadOutput(format!("unknown hunk id {id}")));
            }
            if !seen.insert(id.clone()) {
                return Err(crate::Error::BadOutput(format!("hunk {id} assigned twice")));
            }
        }
    }
    if seen.len() != known.len() {
        return Err(crate::Error::BadOutput(format!(
            "plan covers {} of {} hunks",
            seen.len(),
            known.len()
        )));
    }
    Ok(plan)
}

/// Strip a leading/trailing markdown ``` fence (``` or ```json) if present.
fn strip_code_fence(s: &str) -> &str {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.strip_prefix("json").unwrap_or(rest);
        let rest = rest.trim_start_matches('\n');
        return rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_message_prompt_honors_convention() {
        let style = CommitStyle {
            conventional: true,
            recent: vec!["feat: a".into(), "fix: b".into()],
        };
        let (sys, prompt) = commit_message("diff", &style);
        assert!(sys.contains("Conventional Commits"));
        assert!(prompt.contains("feat: a"));
        assert!(prompt.contains("Staged diff:"));
    }

    #[test]
    fn explain_prompt_varies_by_target() {
        let (sys, _) = explain(ExplainTarget::Stash, "x");
        assert!(sys.contains("stashed changes"));
        let (sys, _) = explain(ExplainTarget::Commit, "x");
        assert!(sys.contains("commit"));
    }

    #[test]
    fn conflict_prompt_includes_all_sides() {
        let (sys, p) = resolve_conflict("a.rs", "B", "O", "T");
        assert!(sys.contains("ONLY the resolved code"));
        assert!(p.contains("BASE") && p.contains("OURS") && p.contains("THEIRS"));
        assert!(p.contains("a.rs"));
    }

    #[test]
    fn parses_valid_plan_and_rejects_bad() {
        let ids = vec![
            "a.rs:0".to_string(),
            "a.rs:1".to_string(),
            "b.rs:0".to_string(),
        ];
        let raw = r#"```json
        {"commits":[
          {"message":"feat: a","hunk_ids":["a.rs:0","a.rs:1"]},
          {"message":"fix: b","hunk_ids":["b.rs:0"]}
        ]}
        ```"#;
        let plan = parse_commit_plan(raw, &ids).expect("valid plan");
        assert_eq!(plan.commits.len(), 2);

        // Unknown id rejected.
        let bad = r#"{"commits":[{"message":"x","hunk_ids":["z:9"]}]}"#;
        assert!(parse_commit_plan(bad, &ids).is_err());

        // Incomplete coverage rejected.
        let partial = r#"{"commits":[{"message":"x","hunk_ids":["a.rs:0"]}]}"#;
        assert!(parse_commit_plan(partial, &ids).is_err());
    }
}
