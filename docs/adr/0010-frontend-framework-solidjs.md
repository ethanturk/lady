# Tauri frontend framework: SolidJS + TypeScript + Vite

The Tauri webview frontend ([ADR-0001](0001-gui-framework-tauri.md)) is built with **SolidJS + TypeScript + Vite**.

**Why:** fine-grained reactivity with no virtual-DOM overhead suits Lady's dense, virtualized views and the hybrid canvas commit graph ([ADR-0005](0005-commit-graph-canvas.md)); tiny runtime, fast updates, and a first-class Tauri template. TypeScript gives a typed contract against the Rust `lady-proto` types crossing the bridge.

**Trade-off:** smaller ecosystem than React; accepted in exchange for performance + simplicity under ***ship fast***. Reversible early — the frontend is thin glue over Rust commands, so swapping frameworks before much UI exists is cheap.
