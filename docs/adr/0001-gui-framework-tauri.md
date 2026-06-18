# GUI framework: Tauri v2

Lady optimizes for "ship fast, broad cross-platform parity" over maximal native fidelity, so we build on **Tauri v2** (Rust core + web frontend) rather than a pure-Rust GUI (egui / Slint / iced).

**Why:** reaches Fork-level UI fastest, gives rich virtualized views (commit graph, diff, trees) and theming cheaply, and ships to macOS / Windows / Linux from one codebase. The trade-off accepted: a system-WebView dependency and a JS layer, instead of Fork's native-tiny footprint.

**Hedge:** all git/AI logic lives in GUI-agnostic Rust crates (`lady-*`), so a native shell (egui/Slint) can replace the webview later if native fidelity becomes the priority — without touching domain logic.
