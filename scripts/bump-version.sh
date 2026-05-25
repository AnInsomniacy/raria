#!/usr/bin/env bash
# Bump the Cargo workspace version for raria.
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  ./scripts/bump-version.sh <major.minor.patch>

Example:
  ./scripts/bump-version.sh 1.0.0
USAGE
}

if [ "$#" -ne 1 ]; then
  usage >&2
  exit 1
fi

VERSION="$1"

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Invalid version: $VERSION" >&2
  echo "Expected plain Semantic Versioning: major.minor.patch" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CARGO_TOML="$PROJECT_ROOT/Cargo.toml"
CARGO_LOCK="$PROJECT_ROOT/Cargo.lock"

cd "$PROJECT_ROOT"

if [ ! -f "$CARGO_TOML" ]; then
  echo "Unable to find Cargo.toml at $CARGO_TOML" >&2
  exit 1
fi

STATUS="$(git status --porcelain)"
if [ -n "$STATUS" ]; then
  DISALLOWED="$(
    printf '%s\n' "$STATUS" |
      awk '$2 != "Cargo.toml" && $2 != "Cargo.lock" { print }'
  )"
  if [ -n "$DISALLOWED" ]; then
    echo "Version bump requires a clean tree except Cargo.toml/Cargo.lock." >&2
    printf '%s\n' "$DISALLOWED" >&2
    exit 1
  fi
fi

if ! git diff --cached --quiet -- Cargo.toml Cargo.lock; then
  echo "Cargo.toml or Cargo.lock has staged changes." >&2
  echo "Unstage version files before bumping again." >&2
  exit 1
fi

CURRENT_VERSION="$(
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

if [ -z "$CURRENT_VERSION" ]; then
  echo "Unable to read [workspace.package] version from Cargo.toml" >&2
  exit 1
fi

if [ "$CURRENT_VERSION" = "$VERSION" ]; then
  echo "Workspace version is already $VERSION"
  exit 0
fi

RARIA_NEW_VERSION="$VERSION" perl -0pi -e '
  my $version = $ENV{"RARIA_NEW_VERSION"};
  s/(\[workspace\.package\]\n(?:[^\[]*\n)*?version = ")[0-9]+\.[0-9]+\.[0-9]+(")/$1$version$2/s
' "$CARGO_TOML"

cargo metadata --format-version 1 >/dev/null

echo "Bumped raria workspace version: $CURRENT_VERSION -> $VERSION"
