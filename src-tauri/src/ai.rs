//! AI commands (Phase 5) — BYOK key management, consent, per-repo toggle, and
//! the user-facing AI features. Keys live in the OS keychain (ADR-0008); the
//! consent gate + redaction live in `lady-ai` ([`lady_ai::complete_guarded`]),
//! so no remote call here can bypass them (ADR-0009).

use std::collections::HashMap;
use std::sync::Mutex;

use lady_ai::context::{self, Budget};
use lady_ai::{AiConfig, AiRequest, AiTask, CancelToken, ProviderKind, StreamSink};
use lady_proto::RepoId;
use tauri::{AppHandle, Emitter, State};

use crate::{load_settings_inner, repo_settings_key, update_settings_inner, GixEngine};
use lady_git::{CommitOpts, DiffSpec, GitEngine};

/// Managed AI state: the keychain-backed key store + in-flight cancel tokens.
pub struct AiState {
    /// API keys, stored in the OS keychain under provider [`ProviderKind::key_id`].
    pub keys: Box<dyn lady_hosting::TokenStore>,
    /// Cancel tokens for streaming completions, keyed by the UI's request id.
    pub cancels: Mutex<HashMap<String, CancelToken>>,
}

impl AiState {
    /// Build with the real OS keychain store.
    pub fn new() -> Self {
        AiState {
            keys: Box::new(lady_hosting::KeyringStore::new("Lady-AI")),
            cancels: Mutex::new(HashMap::new()),
        }
    }

    fn register(&self, req_id: &str) -> CancelToken {
        let tok = CancelToken::new();
        self.cancels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(req_id.to_string(), tok.clone());
        tok
    }

    fn unregister(&self, req_id: &str) {
        self.cancels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(req_id);
    }
}

impl Default for AiState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Config / keys / consent / per-repo toggle (PH5-002) ─────────────────────────

/// The persisted AI config (no secrets — keys live in the keychain).
#[tauri::command]
pub fn ai_get_config() -> Result<AiConfig, String> {
    Ok(load_settings_inner().ai)
}

/// Persist the AI config (provider, models, endpoints). Consent is managed by
/// [`ai_grant_consent`]; it is preserved here so a config save cannot silently
/// grant or revoke it. The per-repo AI toggle and overrides are also preserved
/// so saving provider/model settings never disables AI for a repo.
#[tauri::command]
pub fn ai_set_config(config: AiConfig) -> Result<(), String> {
    update_settings_inner(|settings| {
        let consented = settings.ai.consented.clone();
        let ai_disabled_repos = settings.ai_disabled_repos.clone();
        let repo_overrides = settings.repo_overrides.clone();
        settings.ai = config;
        settings.ai.consented = consented;
        settings.ai_disabled_repos = ai_disabled_repos;
        settings.repo_overrides = repo_overrides;
        Ok(())
    })
}

/// Store an API key for `provider` in the OS keychain. Never written to disk
/// plaintext, never logged (ADR-0008).
#[tauri::command]
pub fn ai_set_key(
    provider: ProviderKind,
    key: String,
    ai: State<'_, AiState>,
) -> Result<(), String> {
    let id = provider
        .key_id()
        .ok_or_else(|| "the local provider needs no key".to_string())?;
    ai.keys.set(id, key.trim()).map_err(|e| e.to_string())
}

/// Delete a stored API key (idempotent).
#[tauri::command]
pub fn ai_delete_key(provider: ProviderKind, ai: State<'_, AiState>) -> Result<(), String> {
    let Some(id) = provider.key_id() else {
        return Ok(());
    };
    ai.keys.delete(id).map_err(|e| e.to_string())
}

/// Whether an API key is stored for `provider` (the local provider is always
/// "ready"). Never returns the key itself.
#[tauri::command]
pub fn ai_has_key(provider: ProviderKind, ai: State<'_, AiState>) -> Result<bool, String> {
    let Some(id) = provider.key_id() else {
        return Ok(true);
    };
    Ok(ai.keys.get(id).map_err(|e| e.to_string())?.is_some())
}

/// Record explicit remote-send consent for `provider` (ADR-0009). The first AI
/// action that would call a remote provider is blocked until this is called.
#[tauri::command]
pub fn ai_grant_consent(provider: ProviderKind) -> Result<(), String> {
    if !provider.is_remote() {
        return Ok(());
    }
    update_settings_inner(|settings| {
        if !settings.ai.consented.contains(&provider) {
            settings.ai.consented.push(provider);
        }
        Ok(())
    })
}

/// Revoke consent for `provider`.
#[tauri::command]
pub fn ai_revoke_consent(provider: ProviderKind) -> Result<(), String> {
    update_settings_inner(|settings| {
        settings.ai.consented.retain(|p| *p != provider);
        Ok(())
    })
}

fn repo_key(repo: &RepoId, engine: &GixEngine) -> Result<String, String> {
    repo_settings_key(repo, engine)
}

/// Enable or disable AI for a repo. AI is on by default (ADR-0009); this records
/// an explicit opt-out in `ai_disabled_repos`.
#[tauri::command]
pub fn ai_set_repo_enabled(
    repo: RepoId,
    enabled: bool,
    engine: State<'_, GixEngine>,
) -> Result<(), String> {
    let key = repo_key(&repo, &engine)?;
    update_settings_inner(|settings| {
        settings.ai_disabled_repos.retain(|p| p != &key);
        if !enabled {
            settings.ai_disabled_repos.push(key);
        }
        Ok(())
    })
}

/// The per-repo AI model override for `repo`, if any (keyed like `ai_disabled_repos`).
/// Used to steer model selection in [`run_task`] before the global default.
fn repo_ai_model(repo: &RepoId, engine: &GixEngine) -> Option<String> {
    let key = repo_key(repo, engine).ok()?;
    load_settings_inner()
        .repo_overrides
        .get(&key)
        .and_then(|o| o.ai_model.clone())
}

/// Whether AI is enabled for `repo`. On by default unless explicitly opted out.
#[tauri::command]
pub fn ai_repo_enabled(repo: RepoId, engine: State<'_, GixEngine>) -> Result<bool, String> {
    let key = repo_key(&repo, &engine)?;
    Ok(!load_settings_inner().ai_disabled_repos.contains(&key))
}

/// List models from the configured OpenAI-compatible server (`/models`). The
/// API key is optional (local servers ignore it).
#[tauri::command]
pub async fn ai_list_models(ai: State<'_, AiState>) -> Result<Vec<String>, String> {
    let cfg = load_settings_inner().ai;
    let key = ProviderKind::OpenAiCompatible
        .key_id()
        .and_then(|id| ai.keys.get(id).ok().flatten())
        .unwrap_or_default();
    lady_ai::OpenAiProvider::with_base_url(cfg.openai_base_url.trim_end_matches('/'), key)
        .list_models()
        .await
        .map_err(|e| e.to_string())
}

/// Cancel an in-flight streaming completion by its request id.
#[tauri::command]
pub fn ai_cancel(req_id: String, ai: State<'_, AiState>) -> Result<(), String> {
    if let Some(tok) = ai
        .cancels
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&req_id)
    {
        tok.cancel();
    }
    Ok(())
}

// ── Shared task runner ──────────────────────────────────────────────────────────

/// Confirm AI is enabled for `repo` (its family id is NOT in `ai_disabled_repos`,
/// the opt-out list — AI is on by default); returns the selected workdir path for
/// context collection.
fn require_repo_enabled(repo: &RepoId, engine: &GixEngine) -> Result<String, String> {
    let wd = engine
        .workdir_path(repo)
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .to_string();
    let key = repo_key(repo, engine)?;
    if load_settings_inner().ai_disabled_repos.contains(&key) {
        return Err("AI is off for this repository — enable it in Settings.".to_string());
    }
    Ok(wd)
}

/// Run a built (system, prompt) request for `task` through the active provider,
/// streaming deltas to the `event` Tauri channel and returning the full text.
/// Enforces consent + redaction via [`lady_ai::complete_guarded`].
#[allow(clippy::too_many_arguments)]
async fn run_task(
    app: &AppHandle,
    ai: &AiState,
    task: AiTask,
    system: String,
    prompt: String,
    temperature: f32,
    event: String,
    req_id: String,
    repo_model: Option<String>,
) -> Result<String, String> {
    let cfg = load_settings_inner().ai;
    let kind = cfg
        .active
        .ok_or_else(|| "No AI provider selected — choose one in Settings.".to_string())?;

    let api_key = match kind.key_id() {
        Some(id) => ai.keys.get(id).map_err(|e| e.to_string())?,
        None => None,
    };
    let provider = lady_ai::build_provider(kind, &cfg, api_key).map_err(|e| e.to_string())?;

    // Model precedence: explicit per-task override > per-repo override > global
    // default > the provider's built-in default.
    let model = match repo_model {
        Some(m) if !m.is_empty() && !cfg.models.contains_key(task.id()) => m,
        _ => cfg.model_for(task),
    };
    let mut req = AiRequest::new(task, model);
    req.system = system;
    req.prompt = prompt;
    req.temperature = temperature;

    let cancel = ai.register(&req_id);
    let app2 = app.clone();
    let ev = event.clone();
    let mut on_token = move |d: &str| {
        let _ = app2.emit(&ev, d.to_string());
    };
    let mut sink = StreamSink::new(&mut on_token, cancel);
    let result = lady_ai::complete_guarded(provider.as_ref(), kind, &cfg, req, &mut sink).await;
    ai.unregister(&req_id);

    let resp = result.map_err(|e| e.to_string())?;
    Ok(resp.text)
}

/// A token budget for the active provider's context window.
fn active_budget() -> Budget {
    let cfg = load_settings_inner().ai;
    let window = cfg
        .active
        .map(|k| match k {
            // User-configurable — local models range from 8k to 128k+.
            ProviderKind::OpenAiCompatible => cfg.openai_context_window.max(2048),
            ProviderKind::AnthropicCompatible => cfg.anthropic_context_window.max(2048),
            ProviderKind::Mistral => 32_000,
            ProviderKind::Anthropic => 200_000,
            ProviderKind::Gemini => 1_000_000,
            ProviderKind::OpenAi | ProviderKind::AzureOpenAi => 128_000,
        })
        .unwrap_or(8192);
    Budget::for_context_window(window)
}

/// Collect the staged diff (HEAD↔index) for all staged paths, budgeted to text.
fn staged_diff_text(repo: &RepoId, engine: &GixEngine) -> Result<String, String> {
    let wt = engine.status(repo).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for f in &wt.staged {
        if let Some(d) = engine
            .diff_spec(repo, &DiffSpec::IndexVsHead(f.path.clone()))
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
        {
            files.push(d);
        }
    }
    Ok(context::budget_diff(&files, active_budget()))
}

/// Collect the working diff (index↔worktree) for unstaged + untracked paths.
pub(crate) fn working_files(
    repo: &RepoId,
    engine: &GixEngine,
) -> Result<Vec<lady_proto::FileDiff>, String> {
    let wt = engine.status(repo).map_err(|e| e.to_string())?;
    let mut paths: Vec<String> = wt.unstaged.iter().map(|f| f.path.clone()).collect();
    paths.extend(wt.untracked.iter().cloned());
    let mut files = Vec::new();
    for p in paths {
        if let Some(d) = engine
            .diff_spec(repo, &DiffSpec::WorkingVsIndex(p))
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
        {
            files.push(d);
        }
    }
    Ok(files)
}

fn working_diff_text(repo: &RepoId, engine: &GixEngine) -> Result<String, String> {
    let files = working_files(repo, engine)?;
    Ok(context::budget_diff(&files, active_budget()))
}

// ── PH5-006: commit message ─────────────────────────────────────────────────────

/// Generate a commit message for the staged changes, streaming to the
/// `ai-stream` event and returning the full text (PH5-006).
#[tauri::command]
pub async fn ai_commit_message(
    repo: RepoId,
    req_id: String,
    engine: State<'_, GixEngine>,
    ai: State<'_, AiState>,
    app: AppHandle,
) -> Result<String, String> {
    require_repo_enabled(&repo, &engine)?;
    let recent = engine
        .recent_messages(&repo, 10)
        .map_err(|e| e.to_string())?;
    let style = context::commit_style(&recent);
    let diff = staged_diff_text(&repo, &engine)?;
    if diff.trim().is_empty() {
        return Err("Nothing staged to describe.".to_string());
    }
    let (system, prompt) = lady_ai::prompts::commit_message(&diff, &style);
    run_task(
        &app,
        &ai,
        AiTask::CommitMessage,
        system,
        prompt,
        0.2,
        "ai-stream".to_string(),
        req_id,
        repo_ai_model(&repo, &engine),
    )
    .await
}

// ── PH5-007: Commit Composer ────────────────────────────────────────────────────

/// Render the working files for the composer, labeling each hunk with a stable
/// `path:index` id, and return the text plus the id list.
fn render_with_ids(files: &[lady_proto::FileDiff]) -> (String, Vec<String>) {
    use lady_proto::LineKind;
    let mut out = String::new();
    let mut ids = Vec::new();
    for f in files {
        for (i, h) in f.hunks.iter().enumerate() {
            let id = format!("{}:{}", f.path, i);
            ids.push(id.clone());
            out.push_str(&format!("--- hunk {id} ({}) ---\n", f.path));
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
    }
    (out, ids)
}

/// Ask the model to split the working changes into logical commits, returning a
/// validated plan for the user to review (PH5-007). Does NOT apply anything.
#[tauri::command]
pub async fn ai_compose_commits(
    repo: RepoId,
    req_id: String,
    engine: State<'_, GixEngine>,
    ai: State<'_, AiState>,
    app: AppHandle,
) -> Result<lady_ai::prompts::CommitPlan, String> {
    require_repo_enabled(&repo, &engine)?;
    let files = working_files(&repo, &engine)?;
    let (text, ids) = render_with_ids(&files);
    if ids.is_empty() {
        return Err("No working changes to organize.".to_string());
    }
    let (system, prompt) = lady_ai::prompts::split_commits(&text, &ids);
    let raw = run_task(
        &app,
        &ai,
        AiTask::SplitCommits,
        system,
        prompt,
        0.1,
        "ai-stream".to_string(),
        req_id,
        repo_ai_model(&repo, &engine),
    )
    .await?;
    lady_ai::prompts::parse_commit_plan(&raw, &ids).map_err(|e| e.to_string())
}

/// Apply a reviewed commit plan: stage each group's hunks via the partial-staging
/// patch builder, then commit with that group's message (PH5-007). Never runs
/// without an explicit user confirm in the UI.
#[tauri::command]
pub fn ai_apply_commit_plan(
    repo: RepoId,
    plan: lady_ai::prompts::CommitPlan,
    engine: State<'_, GixEngine>,
) -> Result<usize, String> {
    require_repo_enabled(&repo, &engine)?;
    apply_commit_plan_inner(&repo, &plan, &engine)
}

/// Core of [`ai_apply_commit_plan`] (no per-repo gate) so it is unit-testable.
pub(crate) fn apply_commit_plan_inner(
    repo: &RepoId,
    plan: &lady_ai::prompts::CommitPlan,
    engine: &GixEngine,
) -> Result<usize, String> {
    // Snapshot the working hunks so ids stay stable as we stage/commit.
    let files = working_files(repo, engine)?;
    let by_path: std::collections::HashMap<&str, &lady_proto::FileDiff> =
        files.iter().map(|f| (f.path.as_str(), f)).collect();

    let mut made = 0usize;
    for commit in &plan.commits {
        // Group this commit's hunk ids by file path → hunk indices.
        let mut per_path: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        for id in &commit.hunk_ids {
            let (path, idx) = id
                .rsplit_once(':')
                .ok_or_else(|| format!("malformed hunk id {id}"))?;
            let idx: usize = idx.parse().map_err(|_| format!("bad hunk index in {id}"))?;
            per_path.entry(path.to_string()).or_default().push(idx);
        }
        for (path, mut idxs) in per_path {
            idxs.sort_unstable();
            let file = by_path
                .get(path.as_str())
                .ok_or_else(|| format!("unknown path {path} in plan"))?;
            // Added/deleted/binary/image files have no text hunks that
            // `git apply --cached` can create (no `/dev/null` header), so stage
            // the whole path. Modified files stage just the selected hunks.
            match file.kind {
                lady_proto::FileDiffKind::Modified => {
                    let patch = lady_diff::build_patch(&path, &file.hunks, &idxs);
                    engine
                        .apply_patch(repo, &patch, false, true)
                        .map_err(|e| e.to_string())?;
                }
                _ => {
                    engine
                        .stage_paths(repo, std::slice::from_ref(&path))
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        engine
            .commit(repo, &commit.message, &CommitOpts::default())
            .map_err(|e| e.to_string())?;
        made += 1;
    }
    Ok(made)
}

// ── Recompose commits (AI regroup of a HEAD-attached span) ──────────────────────

/// Result of planning a recompose: the AI's regroup plan plus context the UI
/// needs to warn/confirm before applying.
#[derive(serde::Serialize)]
pub struct RecomposePlan {
    /// The proposed commits (reuses the Composer plan shape).
    pub plan: lady_ai::prompts::CommitPlan,
    /// Whether any commit in the span has already been pushed (force-push needed).
    pub pushed: bool,
    /// How many commits the span currently has (for the "N → M" confirm).
    pub commit_count: usize,
}

/// The recompose base = the parent of `from_oid`. Errors on a root commit (v1
/// cannot recompose the very first commit, which has no parent to reset to).
fn recompose_base(
    repo: &RepoId,
    engine: &GixEngine,
    from_oid: &str,
) -> Result<lady_proto::Oid, String> {
    let meta = engine
        .walk_log(
            repo,
            lady_git::GraphQuery {
                start: Some(lady_proto::Oid(from_oid.to_string())),
                limit: 1,
            },
        )
        .map_err(|e| e.to_string())?;
    let first = meta
        .into_iter()
        .next()
        .ok_or_else(|| format!("commit {from_oid} not found"))?;
    first
        .parents
        .into_iter()
        .next()
        .ok_or_else(|| "Cannot recompose the root commit (it has no parent).".to_string())
}

/// Guard recompose: working tree must be clean and the span `base..HEAD` must be
/// linear (no merge commits). Returns the number of commits in the span.
fn recompose_guard(
    repo: &RepoId,
    engine: &GixEngine,
    base: &lady_proto::Oid,
) -> Result<usize, String> {
    let wt = engine.status(repo).map_err(|e| e.to_string())?;
    if !wt.staged.is_empty() || !wt.unstaged.is_empty() || !wt.untracked.is_empty() {
        return Err("Commit or stash your working changes before recomposing.".to_string());
    }
    let range = format!("{}..HEAD", base.as_str());
    let merges = engine
        .run_custom(
            repo,
            &[
                "git".into(),
                "rev-list".into(),
                "--merges".into(),
                range.clone(),
            ],
        )
        .map_err(|e| e.to_string())?;
    if !merges.stdout.trim().is_empty() {
        return Err(
            "This span contains a merge commit; recompose only linear history.".to_string(),
        );
    }
    let count = engine
        .run_custom(
            repo,
            &["git".into(), "rev-list".into(), "--count".into(), range],
        )
        .map_err(|e| e.to_string())?;
    count
        .stdout
        .trim()
        .parse::<usize>()
        .map_err(|_| "could not count commits in the range".to_string())
}

/// Plan a recompose of the commits from `from_oid` up to HEAD into fewer logical
/// commits. Read-only: computes the span's net diff and asks the model to
/// regroup it; does NOT touch history (that is [`ai_recompose_apply`]).
#[tauri::command]
pub async fn ai_recompose_plan(
    repo: RepoId,
    from_oid: String,
    req_id: String,
    engine: State<'_, GixEngine>,
    ai: State<'_, AiState>,
    app: AppHandle,
) -> Result<RecomposePlan, String> {
    require_repo_enabled(&repo, &engine)?;
    let base = recompose_base(&repo, &engine, &from_oid)?;
    let commit_count = recompose_guard(&repo, &engine, &base)?;
    if commit_count == 0 {
        return Err("No commits to recompose.".to_string());
    }
    let head = engine.head_commit(&repo).map_err(|e| e.to_string())?;
    let files = engine
        .diff_range(&repo, &base, &head)
        .map_err(|e| e.to_string())?;
    let (text, ids) = render_with_ids(&files);
    if ids.is_empty() {
        return Err("These commits have no net changes to recompose.".to_string());
    }
    let (system, prompt) = lady_ai::prompts::split_commits(&text, &ids);
    let raw = run_task(
        &app,
        &ai,
        AiTask::SplitCommits,
        system,
        prompt,
        0.1,
        "ai-stream".to_string(),
        req_id,
        repo_ai_model(&repo, &engine),
    )
    .await?;
    let plan = lady_ai::prompts::parse_commit_plan(&raw, &ids).map_err(|e| e.to_string())?;
    let pushed = engine
        .commit_is_pushed(&repo, &lady_proto::Oid(from_oid.clone()))
        .map_err(|e| e.to_string())?;
    Ok(RecomposePlan {
        plan,
        pushed,
        commit_count,
    })
}

/// Apply a reviewed recompose plan: mixed-reset the span to its base so the net
/// change becomes working changes, then stage+commit each planned group. On any
/// failure the original history is restored (`reset --hard` to the prior HEAD).
/// Never runs without an explicit user confirm in the UI.
#[tauri::command]
pub fn ai_recompose_apply(
    repo: RepoId,
    from_oid: String,
    plan: lady_ai::prompts::CommitPlan,
    engine: State<'_, GixEngine>,
) -> Result<usize, String> {
    require_repo_enabled(&repo, &engine)?;
    recompose_apply_inner(&repo, &from_oid, &plan, &engine)
}

/// Core of [`ai_recompose_apply`] (no per-repo gate) so it is unit-testable.
pub(crate) fn recompose_apply_inner(
    repo: &RepoId,
    from_oid: &str,
    plan: &lady_ai::prompts::CommitPlan,
    engine: &GixEngine,
) -> Result<usize, String> {
    let base = recompose_base(repo, engine, from_oid)?;
    recompose_guard(repo, engine, &base)?;
    let orig = engine.head_commit(repo).map_err(|e| e.to_string())?;
    engine
        .reset(repo, &base, lady_proto::ResetMode::Mixed)
        .map_err(|e| e.to_string())?;
    match apply_commit_plan_inner(repo, plan, engine) {
        Ok(made) => Ok(made),
        Err(e) => {
            // Roll the original commits back so a failure leaves history intact.
            let _ = engine.reset(repo, &orig, lady_proto::ResetMode::Hard);
            Err(format!("Recompose failed and was rolled back: {e}"))
        }
    }
}

// ── PH5-008: Explain ────────────────────────────────────────────────────────────

/// What to explain (PH5-008).
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExplainArg {
    /// A single commit by oid.
    Commit { oid: String },
    /// A user-selected set of commits by oid.
    Commits { oids: Vec<String> },
    /// A branch range `base..head`.
    BranchRange { base: String, head: String },
    /// A stash entry by index.
    Stash { index: usize },
    /// The current working changes.
    Working,
    /// A single file's uncommitted changes (working + staged vs HEAD).
    Path { path: String },
    /// A raw diff snippet (e.g. a single hunk) supplied by the UI.
    Diff { diff: String },
}

/// Explain a target in plain English (PH5-008). `regenerate` re-rolls at a
/// higher temperature.
#[tauri::command]
pub async fn ai_explain(
    repo: RepoId,
    target: ExplainArg,
    regenerate: bool,
    req_id: String,
    engine: State<'_, GixEngine>,
    ai: State<'_, AiState>,
    app: AppHandle,
) -> Result<String, String> {
    require_repo_enabled(&repo, &engine)?;
    use lady_ai::prompts::ExplainTarget;
    let (etarget, content) = match target {
        ExplainArg::Commit { oid } => {
            let files = engine
                .diff_commit(&repo, &lady_proto::Oid(oid.clone()))
                .map_err(|e| e.to_string())?;
            let subjects = engine
                .recent_messages(&repo, 1)
                .ok()
                .and_then(|m| m.into_iter().next())
                .unwrap_or_default();
            (
                ExplainTarget::Commit,
                format!(
                    "Commit {oid}: {subjects}\n\n{}",
                    context::budget_diff(&files, active_budget())
                ),
            )
        }
        ExplainArg::Commits { oids } => {
            if oids.is_empty() {
                return Err("No commits selected to explain.".to_string());
            }
            // Share the context budget across the selected commits.
            let full = active_budget();
            let n = oids.len();
            let per = Budget {
                max_tokens: (full.max_tokens / n).max(256),
                max_bytes: (full.max_bytes / n).max(1024),
            };
            let mut content = String::new();
            for oid in &oids {
                let o = lady_proto::Oid(oid.clone());
                let summary = engine
                    .walk_log(
                        &repo,
                        lady_git::GraphQuery {
                            start: Some(o.clone()),
                            limit: 1,
                        },
                    )
                    .ok()
                    .and_then(|m| m.into_iter().next())
                    .map(|m| m.summary)
                    .unwrap_or_default();
                let files = engine.diff_commit(&repo, &o).map_err(|e| e.to_string())?;
                content.push_str(&format!(
                    "===== Commit {oid}: {summary} =====\n{}\n\n",
                    context::budget_diff(&files, per)
                ));
            }
            (ExplainTarget::Commits, content)
        }
        ExplainArg::Working => (
            ExplainTarget::WorkingChanges,
            working_diff_text(&repo, &engine)?,
        ),
        ExplainArg::Path { path } => {
            // Working + staged changes for one file (everything vs HEAD).
            let out = engine
                .run_custom(
                    &repo,
                    &[
                        "git".into(),
                        "diff".into(),
                        "HEAD".into(),
                        "--".into(),
                        path.clone(),
                    ],
                )
                .map_err(|e| e.to_string())?;
            let diff = out.stdout.trim();
            if diff.is_empty() {
                return Err(format!("No uncommitted changes in {path} to explain."));
            }
            (ExplainTarget::Changes, format!("File {path}:\n{diff}"))
        }
        ExplainArg::Diff { diff } => {
            if diff.trim().is_empty() {
                return Err("Nothing to explain.".to_string());
            }
            (ExplainTarget::Changes, diff)
        }
        ExplainArg::Stash { index } => {
            let out = engine
                .run_custom(
                    &repo,
                    &[
                        "git".into(),
                        "stash".into(),
                        "show".into(),
                        "-p".into(),
                        format!("stash@{{{index}}}"),
                    ],
                )
                .map_err(|e| e.to_string())?;
            (ExplainTarget::Stash, out.stdout)
        }
        ExplainArg::BranchRange { base, head } => {
            let log = engine
                .run_custom(
                    &repo,
                    &[
                        "git".into(),
                        "log".into(),
                        "--pretty=%h %s".into(),
                        format!("{base}..{head}"),
                    ],
                )
                .map_err(|e| e.to_string())?;
            let diff = engine
                .run_custom(
                    &repo,
                    &["git".into(), "diff".into(), format!("{base}...{head}")],
                )
                .map_err(|e| e.to_string())?;
            (
                ExplainTarget::BranchRange,
                format!("Commits:\n{}\n\nDiff:\n{}", log.stdout, diff.stdout),
            )
        }
    };
    let (system, prompt) = lady_ai::prompts::explain(etarget, &content);
    let temp = if regenerate { 0.8 } else { 0.3 };
    run_task(
        &app,
        &ai,
        AiTask::Explain,
        system,
        prompt,
        temp,
        "ai-stream".to_string(),
        req_id,
        repo_ai_model(&repo, &engine),
    )
    .await
}

// ── PH5-009: AI conflict resolution (review-gated) ──────────────────────────────

/// Propose a resolution for a conflicted file, streamed for the user to review
/// in the 3-pane resolver (PH5-009). NEVER writes the file — the UI applies it
/// only on explicit confirm.
#[tauri::command]
pub async fn ai_resolve_conflict(
    repo: RepoId,
    path: String,
    req_id: String,
    engine: State<'_, GixEngine>,
    ai: State<'_, AiState>,
    app: AppHandle,
) -> Result<String, String> {
    require_repo_enabled(&repo, &engine)?;
    let sides = engine
        .conflict_sides(&repo, &path)
        .map_err(|e| e.to_string())?;
    let (system, prompt) = lady_ai::prompts::resolve_conflict(
        &path,
        sides.base.as_deref().unwrap_or(""),
        sides.ours.as_deref().unwrap_or(""),
        sides.theirs.as_deref().unwrap_or(""),
    );
    run_task(
        &app,
        &ai,
        AiTask::ResolveConflict,
        system,
        prompt,
        0.1,
        "ai-stream".to_string(),
        req_id,
        repo_ai_model(&repo, &engine),
    )
    .await
}

// ── PH5-010: PR title/description, changelog, stash notes ───────────────────────

fn range_subjects(
    repo: &RepoId,
    engine: &GixEngine,
    base: &str,
    head: &str,
) -> Result<Vec<String>, String> {
    let out = engine
        .run_custom(
            repo,
            &[
                "git".into(),
                "log".into(),
                "--pretty=%s".into(),
                format!("{base}..{head}"),
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(out.stdout.lines().map(|s| s.to_string()).collect())
}

fn range_diff(repo: &RepoId, engine: &GixEngine, base: &str, head: &str) -> Result<String, String> {
    let out = engine
        .run_custom(
            repo,
            &["git".into(), "diff".into(), format!("{base}...{head}")],
        )
        .map_err(|e| e.to_string())?;
    // Byte-cap the range diff to the active budget.
    let cap = active_budget().max_bytes;
    let mut s = out.stdout;
    if s.len() > cap {
        s.truncate(cap);
        s.push_str("\n[... diff truncated to fit the model budget ...]\n");
    }
    Ok(s)
}

/// Generate a PR/MR title summarizing the branch range (PH5-010).
#[tauri::command]
pub async fn ai_pr_title(
    repo: RepoId,
    base: String,
    head: String,
    req_id: String,
    engine: State<'_, GixEngine>,
    ai: State<'_, AiState>,
    app: AppHandle,
) -> Result<String, String> {
    require_repo_enabled(&repo, &engine)?;
    let subjects = range_subjects(&repo, &engine, &base, &head)?;
    let diff = range_diff(&repo, &engine, &base, &head)?;
    let (system, prompt) = lady_ai::prompts::pr_title(&subjects, &diff);
    run_task(
        &app,
        &ai,
        AiTask::PrTitle,
        system,
        prompt,
        0.3,
        "ai-stream".to_string(),
        req_id,
        repo_ai_model(&repo, &engine),
    )
    .await
}

/// Generate a PR/MR description summarizing the branch range (PH5-010).
#[tauri::command]
pub async fn ai_pr_description(
    repo: RepoId,
    base: String,
    head: String,
    req_id: String,
    engine: State<'_, GixEngine>,
    ai: State<'_, AiState>,
    app: AppHandle,
) -> Result<String, String> {
    require_repo_enabled(&repo, &engine)?;
    let subjects = range_subjects(&repo, &engine, &base, &head)?;
    let diff = range_diff(&repo, &engine, &base, &head)?;
    let (system, prompt) = lady_ai::prompts::pr_description(&subjects, &diff);
    run_task(
        &app,
        &ai,
        AiTask::PrDescription,
        system,
        prompt,
        0.3,
        "ai-stream".to_string(),
        req_id,
        repo_ai_model(&repo, &engine),
    )
    .await
}

/// Generate a changelog grouping the range by Conventional-Commit type (PH5-010).
#[tauri::command]
pub async fn ai_changelog(
    repo: RepoId,
    base: String,
    head: String,
    req_id: String,
    engine: State<'_, GixEngine>,
    ai: State<'_, AiState>,
    app: AppHandle,
) -> Result<String, String> {
    require_repo_enabled(&repo, &engine)?;
    let subjects = range_subjects(&repo, &engine, &base, &head)?;
    let (system, prompt) = lady_ai::prompts::changelog(&subjects);
    run_task(
        &app,
        &ai,
        AiTask::Changelog,
        system,
        prompt,
        0.2,
        "ai-stream".to_string(),
        req_id,
        repo_ai_model(&repo, &engine),
    )
    .await
}

/// Summarize working changes into a short stash note (PH5-010).
#[tauri::command]
pub async fn ai_stash_note(
    repo: RepoId,
    req_id: String,
    engine: State<'_, GixEngine>,
    ai: State<'_, AiState>,
    app: AppHandle,
) -> Result<String, String> {
    require_repo_enabled(&repo, &engine)?;
    let diff = working_diff_text(&repo, &engine)?;
    if diff.trim().is_empty() {
        return Err("Nothing to summarize.".to_string());
    }
    let (system, prompt) = lady_ai::prompts::stash_note(&diff);
    run_task(
        &app,
        &ai,
        AiTask::StashNote,
        system,
        prompt,
        0.3,
        "ai-stream".to_string(),
        req_id,
        repo_ai_model(&repo, &engine),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use lady_ai::prompts::{CommitPlan, PlannedCommit};
    use lady_git::GitEngine;
    use std::path::Path;
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .expect("git installed")
            .success();
        assert!(ok, "git {args:?} failed");
    }

    /// A 2-file working tree split into 2 commits yields exactly 2 commits and a
    /// clean tree (PH5-007 acceptance) — exercises the partial-staging apply path
    /// with no AI involved.
    #[test]
    fn apply_commit_plan_makes_expected_commits() {
        let dir = TempDir::new().expect("tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "T"]);
        git(p, &["config", "user.email", "t@t.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);
        std::fs::write(p.join("a.txt"), "base\n").unwrap();
        std::fs::write(p.join("b.txt"), "base\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "init"]);

        // Two independent working changes.
        std::fs::write(p.join("a.txt"), "base\nchange a\n").unwrap();
        std::fs::write(p.join("b.txt"), "base\nchange b\n").unwrap();

        let engine = GixEngine::new();
        let repo = engine.open(p).expect("open");

        let plan = CommitPlan {
            commits: vec![
                PlannedCommit {
                    message: "feat: a".into(),
                    hunk_ids: vec!["a.txt:0".into()],
                },
                PlannedCommit {
                    message: "feat: b".into(),
                    hunk_ids: vec!["b.txt:0".into()],
                },
            ],
        };
        let made = apply_commit_plan_inner(&repo, &plan, &engine).expect("apply");
        assert_eq!(made, 2, "two commits created");

        // History now has init + 2 = 3 commits; working tree clean.
        let log = engine
            .run_custom(&repo, &["git".into(), "log".into(), "--pretty=%s".into()])
            .expect("log");
        let subjects: Vec<&str> = log.stdout.lines().collect();
        assert_eq!(subjects, vec!["feat: b", "feat: a", "init"]);
        assert!(
            !engine.is_dirty(&repo).expect("dirty"),
            "tree should be clean"
        );
    }
}
