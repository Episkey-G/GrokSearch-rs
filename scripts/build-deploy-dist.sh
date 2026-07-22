#!/usr/bin/env bash
# Cross-compile the static musl binaries Dockerfile.deploy expects and lay
# them out as dist/<docker-platform>/grok-search-rs:
#   dist/linux/amd64/grok-search-rs
#   dist/linux/arm64/grok-search-rs
#
# Usage:
#   scripts/build-deploy-dist.sh               # every supported platform
#   scripts/build-deploy-dist.sh linux/arm64   # a subset
#
# Requires cargo-zigbuild (cargo install cargo-zigbuild, or brew install cargo-zigbuild).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

if ! command -v cargo-zigbuild >/dev/null 2>&1; then
  echo "cargo-zigbuild not found; install with: cargo install cargo-zigbuild (or brew install cargo-zigbuild)" >&2
  exit 69
fi

rust_target() {
  case "$1" in
    linux/amd64) echo "x86_64-unknown-linux-musl" ;;
    linux/arm64) echo "aarch64-unknown-linux-musl" ;;
    *) echo "unsupported platform '$1' (supported: linux/amd64 linux/arm64)" >&2; return 64 ;;
  esac
}

PLATFORMS=("$@")
if [[ ${#PLATFORMS[@]} -eq 0 ]]; then
  PLATFORMS=(linux/amd64 linux/arm64)
fi

for platform in "${PLATFORMS[@]}"; do
  target="$(rust_target "$platform")"
  echo "==> $platform ($target)"
  rustup target add "$target"
  cargo zigbuild --profile release-http --features http --target "$target" --locked
  mkdir -p "dist/$platform"
  # install(1), not cp: pin mode 0755 regardless of umask — docker COPY keeps
  # the staged mode and the image runs as non-root, so 0700 here bricks it.
  install -m 0755 "target/$target/release-http/grok-search-rs" "dist/$platform/grok-search-rs"
done

echo "==> dist/ ready:"
ls -l dist/*/*/grok-search-rs
