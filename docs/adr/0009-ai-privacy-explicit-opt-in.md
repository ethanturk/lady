# AI privacy: explicit opt-in, no silent cloud

Lady never sends repository code to a remote AI provider without explicit, informed consent.

- **First-use consent, per provider.** The first AI action that would call a remote provider shows which provider/model will receive data and that code/diffs leave the machine. No silent default to cloud.
- **Per-repo AI toggle, default off until configured.** A sensitive repository can stay AI-off entirely.
- **Local path is first-class.** Ollama is surfaced prominently as the "never leaves your machine" option.
- **Secret-redaction pass before any remote send** (entropy + regex scan for obvious credentials). This is **best-effort, not a guarantee** — it reduces accidental leakage, it does not make sending code safe by itself, and the UI must not imply otherwise.
- **Minimization.** Token-budgeting / diff-truncation limits payload size; prompts and responses containing code are **not logged** by default.

**Why:** the product is closed-source commercial ([[closed-source-commercial]]) handling proprietary code under BYOK ([[ai-byok-local-ollama]]); a silent or opt-out posture would be an unacceptable trust and compliance risk.
