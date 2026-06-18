//! `lady-mcp` binary — a read-only MCP server over stdio (PH5-011).
//!
//! An external assistant (Claude/Cursor/etc.) launches this with a repository
//! path; Lady then serves repo context (status/diff/log/file/blame/search) over
//! the Model Context Protocol. Enable/disable is controlled by the assistant's
//! MCP config (add/remove this entry); the exposed repo is the path argument.
//!
//! Usage: `lady-mcp [REPO_PATH]` (defaults to the current directory).

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
    lady_mcp::serve_stdio(&path).await
}
