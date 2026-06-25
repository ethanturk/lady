# Lady — AI & Privacy

Lady's AI features (commit messages, Commit Composer, explain, conflict help, PR
title/description, changelog, stash notes) are **bring-your-own-key (BYOK)** with
**no hosted Lady backend** (ADR-0008). Your code and keys go only to the provider
*you* choose. Privacy is **explicit opt-in** and enforced in the engine
(ADR-0009).

## How it works

- **BYOK keys** live in your **OS keychain** (service `Lady-AI`) — never on disk
  in plaintext, never in logs. Manage them in **Settings → AI**.
- **Per-repo toggle.** AI is **on by default**, with a per-repo opt-out for
  sensitive repos (**Settings → Repository → AI**). This toggle is *not* what
  keeps your code private — the remote-consent gate below is. Enabling a repo
  never sends data anywhere by itself.
- **First-use consent, per remote provider.** Before Lady ever calls a *remote*
  provider it requires a recorded, per-provider consent. Until then, remote calls
  are blocked.
- **Local Ollama path** (`http://localhost:11434`): a fully local option that
  **never leaves your machine**. It runs with no consent gate and no mandatory
  redaction, because nothing is sent anywhere. If Ollama isn't running, Lady says
  so plainly.
- **Remote providers:** OpenAI, Anthropic, Gemini, Azure OpenAI, Mistral —
  thin `reqwest` clients (ADR-0011), each over TLS.

## Redaction is best-effort

Before any **remote** send, Lady runs a **best-effort** secret redaction pass
(regex + entropy heuristics) over the context it builds, plus token-budget
truncation. This reduces the chance of leaking obvious secrets (keys, tokens),
but **it is not a guarantee** — treat anything you send to a remote provider as
disclosed to that provider. The local Ollama path does not redact (nothing
leaves the machine).

## What gets sent

Only the context needed for the requested task — e.g. a diff for a commit
message, the working changes for the Composer, a commit/range/stash for explain.
Context is bounded by a token budget; oversized inputs are truncated with a note
rather than silently dropped.

## Review-gated, never auto-applied

AI **suggests**; you decide. The Commit Composer plan, conflict resolutions, and
generated messages are all shown for review and edit — nothing is committed or
written to your tree automatically.

## Turning it off

- Disable AI for a repo: **Settings → Repository → AI → per-repo toggle**.
- Revoke a provider's consent or delete its key: **Settings → AI**.

See ADR-0008 (BYOK + local Ollama) and ADR-0009 (explicit opt-in + redaction)
for the design rationale.
