# Lady — MCP server (`lady-mcp`)

Lady ships a **read-only Model Context Protocol (MCP) server**, `lady-mcp`
(PH5-011). It lets an external AI assistant (Claude Desktop, Cursor, etc.) read
context from one of your repositories. It exposes **no mutating operations** —
nothing it does can change your repo.

## What it exposes

Six read-only tools over stdio:

| Tool | Description |
| --- | --- |
| `get_status` | Working-tree status (staged / unstaged / untracked) |
| `get_diff` | Diff of a commit against its first parent |
| `get_log` | Recent commits, newest first |
| `get_file_at` | A file's contents at a revision |
| `blame` | Per-line last-touching commit for a file |
| `search_commits` | Commits whose summary matches a query (case-insensitive) |

## Running it

```sh
lady-mcp [REPO_PATH]      # defaults to the current directory
```

It speaks MCP over stdio, so an assistant launches it directly. The **exposed
repository is the path argument**; enable/disable is simply adding/removing the
entry in the assistant's MCP config.

## Configuring an assistant

Example MCP server entry (e.g. in a Claude Desktop / Cursor MCP config):

```json
{
  "mcpServers": {
    "lady": {
      "command": "lady-mcp",
      "args": ["/absolute/path/to/your/repo"]
    }
  }
}
```

Point `command` at the `lady-mcp` binary (build it with `cargo build -p lady-mcp`,
or use the one bundled with a release), and `args` at the repository you want the
assistant to read. Restart the assistant to pick up the change.

## Safety

- **Read-only by construction** — the server registers only the six read tools
  above; there are no write/commit/push tools.
- It only ever reads the **one repository** you point it at.
- It runs locally and talks to the assistant over stdio; it opens no network
  listeners of its own.
