# Release strategy: ship at Fork parity, not incrementally

"Ship fast" means **build velocity** (Tauri + the autonomous Ralph loop), not early public release. The first public release gates on full Fork feature parity (PLAN Phases 1–3); no thin read-only MVP ships to external users. GitKraken-style AI (Phase 5) ships *after* parity.

**Why:** the user wants a complete Fork-class first impression, and the autonomous build loop makes "reach parity quickly" realistic without releasing a partial product.

**Consequence / mitigation:** no external feedback until parity — a real risk. Mitigated by mandatory internal dogfooding + green CI + tests at every phase (also required by Ralph's feedback-loop design), so the product is continuously exercised even though the public release is big-bang.
