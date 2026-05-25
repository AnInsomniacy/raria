#!/usr/bin/env bash
# Verify, optionally commit, and tag a local raria release.
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  ./scripts/release.sh [--push]

This script verifies the local release path, creates an optional version commit
for Cargo.toml/Cargo.lock changes, and creates an annotated v{VERSION} tag.

It does not create GitHub Release notes or publish release assets.
It pushes only when --push is passed deliberately.
USAGE
}

PUSH=false

case "${1:-}" in
  "")
    ;;
  --push)
    PUSH=true
    ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage >&2
    exit 1
    ;;
esac

if [ "$#" -gt 1 ]; then
  usage >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CARGO_TOML="$PROJECT_ROOT/Cargo.toml"

cd "$PROJECT_ROOT"

VERSION="$(
  awk '
    /^\[workspace.package\]$/ { in_workspace_package = 1; next }
    /^\[/ { in_workspace_package = 0 }
    in_workspace_package && /^version = "/ {
      gsub(/^version = "/, "", $0)
      gsub(/"$/, "", $0)
      print $0
      exit
    }
  ' "$CARGO_TOML"
)"

if [ -z "$VERSION" ]; then
  echo "Unable to read [workspace.package] version from Cargo.toml" >&2
  exit 1
fi

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Invalid workspace version: $VERSION" >&2
  echo "Expected major.minor.patch." >&2
  exit 1
fi

TAG="v$VERSION"

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  echo "Local tag already exists: $TAG" >&2
  exit 1
fi

if git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1; then
  echo "Remote tag already exists: $TAG" >&2
  exit 1
fi

STATUS="$(git status --porcelain)"
if [ -n "$STATUS" ]; then
  DISALLOWED="$(
    printf '%s\n' "$STATUS" |
      awk '$2 != "Cargo.toml" && $2 != "Cargo.lock" { print }'
  )"
  if [ -n "$DISALLOWED" ]; then
    echo "Release requires a clean tree except Cargo.toml/Cargo.lock version changes." >&2
    printf '%s\n' "$DISALLOWED" >&2
    exit 1
  fi
fi

echo "Preparing $TAG"

cargo fmt --all --check
cargo check --workspace --locked
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --locked -p raria-cli
target/release/raria --version

if ! target/release/raria --version | grep -F "$VERSION" >/dev/null; then
  echo "Release binary version does not contain $VERSION" >&2
  exit 1
fi

if ! git diff --quiet -- Cargo.toml Cargo.lock || ! git diff --cached --quiet -- Cargo.toml Cargo.lock; then
  git add Cargo.toml Cargo.lock
  git commit -m "release: $TAG"
else
  echo "No version files changed. Tagging current HEAD."
fi

git tag -a "$TAG" -m "$TAG"

if [ "$PUSH" = true ]; then
  git push
  git push origin "$TAG"
  echo "Pushed release tag: $TAG"
else
  echo "Created local release tag: $TAG"
  echo "Push deliberately with: git push origin HEAD && git push origin $TAG"
fi
