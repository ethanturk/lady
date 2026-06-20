#!/bin/sh
# Xcode Cloud post-clone hook for the Tauri iOS app.
#
# Xcode Cloud images ship Xcode + Homebrew but NOT Rust, the Tauri CLI, or a
# built frontend. The app's "Build Rust Code" phase runs `cargo tauri ios
# xcode-script`, which compiles the Rust staticlib — and tauri-build embeds the
# web assets from ../../../ui/dist at compile time. So before any build we must:
#   1. install Rust + the aarch64-apple-ios target,
#   2. install the Tauri CLI (provides `cargo tauri`),
#   3. install Node and build ui/dist.
#
# Xcode Cloud runs this script from the ci_scripts directory and does NOT carry
# exported env vars over to the build phase — only files on disk persist. We
# therefore install into ~/.cargo/bin and make the build phase source the cargo
# env (see the preBuildScript in project.yml). Anything installed here is cached
# by Xcode Cloud's dependency cache between runs.
set -eu

REPO="${CI_PRIMARY_REPOSITORY_PATH:?CI_PRIMARY_REPOSITORY_PATH must be set by Xcode Cloud}"

echo "▸ Installing Rust toolchain"
if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
# shellcheck disable=SC1090
. "$HOME/.cargo/env"
rustup target add aarch64-apple-ios

echo "▸ Installing the Tauri CLI"
if ! command -v cargo-tauri >/dev/null 2>&1; then
  cargo install tauri-cli --version "^2" --locked
fi

echo "▸ Installing Node"
if ! command -v node >/dev/null 2>&1; then
  brew install node
fi

echo "▸ Building the frontend (ui/dist)"
cd "$REPO/ui"
npm ci
npm run build

echo "✓ post-clone complete"
