# Licensing gate: offline signed key + trial

The v1.0 paid client gates on an **Ed25519-signed license key verified client-side** (embedded public key; the signing private key stays offline). A **30-day full-feature trial** precedes the gate, mirroring Fork's free evaluation period. No licensing server ships for v1.0.

**Why:** minimal infrastructure, no online dependency at launch, fits ship-fast. Online activation (seat management, revocation) is deferred to Fast-follow if piracy materializes.

**Security caveat (explicit):** a client-side check with an embedded public key is a **commercial speed bump, not DRM** — a determined attacker can patch the binary. This is accepted. Therefore **never gate anything security-sensitive behind the license check**, and never embed secrets that the gate is meant to protect. The gate controls feature access for honest users only.
