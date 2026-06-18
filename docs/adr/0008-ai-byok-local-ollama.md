# AI provisioning: BYOK + local Ollama, no hosted backend

Lady's AI (Phase 5) runs on **the user's own inference**: bring-your-own provider keys or a local model. Lady ships **no inference backend**.

- **Providers:** OpenAI, Anthropic Claude, Google Gemini, Azure OpenAI, Mistral, and **Ollama** (local).
- **Keys:** stored in the OS keychain (`keyring`) — never on disk in plaintext, never logged.
- **Monetization:** the paid commercial tier ([[closed-source-commercial]]) unlocks the AI *features*; the user covers inference cost/keys.

**Why:** matches GitKraken's BYOK model, needs zero server (preserves ship-fast and the offline-licensing stance), and keeps code on-machine for users who choose local Ollama.

**Consequence:** remote providers receive code/diffs, so privacy posture is a first-class decision (see ADR-0009). Lady-hosted convenience inference remains a possible future option, not v1.
