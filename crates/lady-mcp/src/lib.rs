//! `lady-mcp` — a read-only Model Context Protocol server exposing Lady's repo
//! context to external assistants (Claude/Cursor/etc.), matching GitKraken's
//! MCP (PLAN.md §5.4, PH5-011).
//!
//! Tools are **read-only** — there are no mutating operations. The server is
//! built over a single repository (the one Lady has open) via `lady-git` and
//! `lady-diff`.

use std::sync::Arc;

use std::future::Future;

use lady_git::{GitEngine, GixEngine, GraphQuery};
use lady_proto::{Oid, RepoId};
use rmcp::handler::server::tool::Parameters;
use rmcp::handler::server::wrapper::Json;
use rmcp::model::{ErrorData, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use serde::{Deserialize, Serialize};

/// A read-only MCP server over one repository.
#[derive(Clone)]
pub struct LadyMcp {
    engine: Arc<GixEngine>,
    repo: RepoId,
    tool_router: rmcp::handler::server::router::tool::ToolRouter<Self>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct LogParams {
    /// Max commits to return (0 = a sensible default).
    #[serde(default)]
    limit: usize,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct DiffParams {
    /// The commit oid to diff against its first parent.
    commit: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct FileAtParams {
    /// The revision (branch, tag, or oid).
    rev: String,
    /// The repo-relative file path.
    path: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct BlameParams {
    /// The repo-relative file path.
    path: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct SearchParams {
    /// Case-insensitive substring to match against commit summaries.
    query: String,
    /// Max results (0 = a sensible default).
    #[serde(default)]
    limit: usize,
}

/// A compact commit record for MCP consumers.
#[derive(Serialize, schemars::JsonSchema)]
struct CommitRecord {
    oid: String,
    summary: String,
    author: String,
    time: i64,
}

fn err(msg: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(msg.to_string(), None)
}

const DEFAULT_LIMIT: usize = 50;

#[tool_router]
impl LadyMcp {
    /// Build a server exposing the repository at `repo_path`.
    pub fn open(repo_path: &std::path::Path) -> Result<Self, lady_git::Error> {
        let engine = GixEngine::new();
        let repo = engine.open(repo_path)?;
        Ok(LadyMcp {
            engine: Arc::new(engine),
            repo,
            tool_router: Self::tool_router(),
        })
    }

    /// Build over an already-open engine + repo id (shares Lady's engine).
    pub fn new(engine: Arc<GixEngine>, repo: RepoId) -> Self {
        LadyMcp {
            engine,
            repo,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Get the working-tree status (staged/unstaged/untracked).")]
    async fn get_status(&self) -> Result<Json<lady_proto::WorkingTree>, ErrorData> {
        self.engine.status(&self.repo).map(Json).map_err(err)
    }

    #[tool(description = "Get the diff of a commit against its first parent.")]
    async fn get_diff(
        &self,
        Parameters(DiffParams { commit }): Parameters<DiffParams>,
    ) -> Result<Json<Vec<lady_proto::FileDiff>>, ErrorData> {
        self.engine
            .diff_commit(&self.repo, &Oid(commit))
            .map(Json)
            .map_err(err)
    }

    #[tool(description = "List recent commits (newest first).")]
    async fn get_log(
        &self,
        Parameters(LogParams { limit }): Parameters<LogParams>,
    ) -> Result<Json<Vec<CommitRecord>>, ErrorData> {
        let limit = if limit == 0 { DEFAULT_LIMIT } else { limit };
        let commits = self
            .engine
            .walk_log(&self.repo, GraphQuery { start: None, limit })
            .map_err(err)?;
        Ok(Json(commits.iter().map(to_record).collect()))
    }

    #[tool(description = "Read a file's contents at a revision.")]
    async fn get_file_at(
        &self,
        Parameters(FileAtParams { rev, path }): Parameters<FileAtParams>,
    ) -> Result<Json<Option<String>>, ErrorData> {
        self.engine
            .file_at(&self.repo, &rev, &path)
            .map(Json)
            .map_err(err)
    }

    #[tool(description = "Blame a file (per-line last-touching commit).")]
    async fn blame(
        &self,
        Parameters(BlameParams { path }): Parameters<BlameParams>,
    ) -> Result<Json<lady_proto::Blame>, ErrorData> {
        self.engine
            .blame(&self.repo, &path, None)
            .map(Json)
            .map_err(err)
    }

    #[tool(description = "Find commits whose summary contains a query (case-insensitive).")]
    async fn search_commits(
        &self,
        Parameters(SearchParams { query, limit }): Parameters<SearchParams>,
    ) -> Result<Json<Vec<CommitRecord>>, ErrorData> {
        let limit = if limit == 0 { DEFAULT_LIMIT } else { limit };
        let commits = self
            .engine
            .search_commits(&self.repo, &query, limit)
            .map_err(err)?;
        Ok(Json(commits.iter().map(to_record).collect()))
    }
}

/// Serve the read-only MCP server for the repository at `repo_path` over stdio
/// until the client disconnects. This is how an external assistant launches
/// Lady as an MCP context provider (it spawns the `lady-mcp` binary). Read-only:
/// the server exposes no mutating tools.
pub async fn serve_stdio(repo_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let server = LadyMcp::open(repo_path)?;
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let running = rmcp::service::serve_server(server, transport).await?;
    running.waiting().await?;
    Ok(())
}

fn to_record(c: &lady_proto::CommitMeta) -> CommitRecord {
    CommitRecord {
        oid: c.oid.0.clone(),
        summary: c.summary.clone(),
        author: c.author.name.clone(),
        time: c.time,
    }
}

#[tool_handler]
impl ServerHandler for LadyMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Read-only access to a Git repository via Lady: status, diff, log, \
                 file-at-rev, blame, and commit search. No mutating operations."
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::CallToolRequestParam;
    use rmcp::serve_client;
    use rmcp::service::serve_server;
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

    fn fixture() -> TempDir {
        let dir = TempDir::new().expect("tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "T"]);
        git(p, &["config", "user.email", "t@t.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);
        for i in 1..=3 {
            std::fs::write(p.join(format!("f{i}.txt")), format!("{i}\n")).unwrap();
            git(p, &["add", "."]);
            git(p, &["commit", "-q", "-m", &format!("commit {i}")]);
        }
        dir
    }

    /// Smoke test (PH5-011): an in-memory MCP client lists the tools and
    /// `get_log` returns commits for a fixture repo. Wrapped in a timeout so a
    /// protocol regression fails fast instead of hanging CI.
    #[tokio::test]
    async fn client_lists_tools_and_gets_log() {
        let dir = fixture();
        let server = LadyMcp::open(dir.path()).expect("open repo");

        let body = async {
            let (server_t, client_t) = tokio::io::duplex(8192);
            // The initialize handshake needs both event loops live at once, so
            // start the server and client concurrently.
            let (server_res, client_res) =
                tokio::join!(serve_server(server, server_t), serve_client((), client_t));
            let running_server = server_res.expect("serve server");
            let client = client_res.expect("serve client");

            // List tools — all six read-only tools are present.
            let tools = client.list_all_tools().await.expect("list tools");
            let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
            for expected in [
                "get_status",
                "get_diff",
                "get_log",
                "get_file_at",
                "blame",
                "search_commits",
            ] {
                assert!(
                    names.iter().any(|n| n == expected),
                    "missing tool {expected}: {names:?}"
                );
            }

            // Call get_log — returns the three fixture commits.
            let result = client
                .call_tool(CallToolRequestParam {
                    name: "get_log".into(),
                    arguments: serde_json::json!({ "limit": 10 }).as_object().cloned(),
                })
                .await
                .expect("call get_log");
            let json = serde_json::to_string(&result.content).expect("serialize content");
            assert!(json.contains("commit 3"), "log missing commits: {json}");
            assert!(json.contains("commit 1"));

            // Drop both ends — closing the transport tears the services down
            // without a blocking cancel handshake.
            drop(client);
            drop(running_server);
        };

        tokio::time::timeout(std::time::Duration::from_secs(20), body)
            .await
            .expect("MCP round-trip timed out");
    }
}
