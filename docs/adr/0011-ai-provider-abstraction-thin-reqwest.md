# AI provider abstraction: thin `reqwest` trait

`lady-ai` defines a small in-house `AiProvider` trait implemented per provider over `reqwest` (rustls), rather than adopting a multi-provider crate (`genai` / `rig`).

**Why:** full control of streaming, token budgeting, the secret-redaction hook ([ADR-0009](0009-ai-privacy-explicit-opt-in.md)), BYOK key handling ([ADR-0008](0008-ai-byok-local-ollama.md)), and exact request shaping per provider. The providers are simple REST/SSE APIs, so a hand-rolled client is cheap and keeps the `cargo-deny` surface small (no large transitive dep tree to audit for a closed-source product, [ADR-0004](0004-closed-source-commercial.md)).

**Trade-off:** we write each client by hand (OpenAI, Anthropic, Gemini, Azure OpenAI, Mistral, Ollama) instead of getting them free from a crate. Accepted for control + privacy; the trait keeps it reversible — a multi-provider crate could back the trait later if the maintenance cost outweighs the control.
