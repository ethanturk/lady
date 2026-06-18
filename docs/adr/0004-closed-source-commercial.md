# Business model: closed-source commercial (Fork model)

Lady ships as a **paid, closed-source** product like Fork — free trial + paid license — not open-source.

**Why:** explicit product decision; cleanest direct monetization and mirrors the product Lady clones.

**Consequences:**

- **Dependency-license constraint (hard rule):** use only permissive crates (MIT / Apache-2.0 / BSD / MPL-2.0). **No GPL/AGPL/copyleft** crates that would force source disclosure. `libgit2` (via the `git2` crate) is GPLv2 **with a linking exception** that explicitly permits linking into proprietary software — acceptable and documented here. Enforce with `cargo-deny` license checks in CI from Phase 0.
- **Licensing gate is in Core Parity:** v1.0 needs a trial period + license-key validation module (mirroring Fork's free-eval + purchase). Small, but it is release-blocking scope, not Fast-follow.
- **Infra ownership:** payment/licensing, code-signing (macOS notarization, Windows Authenticode), auto-update, and opt-in telemetry are all on us.
- **AI tier:** the GitKraken-style AI is a paid feature *inside* the commercial product (Phase 5), not a separate freemium hook.
