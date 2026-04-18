#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

FILES=(
  "$ROOT/README.md"
  "$ROOT/crates/raria-rpc/src/lib.rs"
  "$ROOT/crates/raria-rpc/src/methods.rs"
  "$ROOT/crates/raria-rpc/src/server.rs"
)

bad=0
for phrase in "full aria2 parity" "complete aria2-compatible" "supports all aria2"; do
  if rg -n -i --fixed-strings "$phrase" "${FILES[@]}" >/dev/null 2>&1; then
    echo "claim drift: found forbidden phrase '$phrase'"
    bad=1
  fi
done

if rg -n --fixed-strings "aria2.onSourceFailed" "$ROOT/README.md" | rg -v "extension-style|extension surface|Current extension surface|extension notification" >/dev/null 2>&1; then
  echo "claim drift: aria2.onSourceFailed must be described as an extension surface"
  bad=1
fi

if [[ "$bad" -ne 0 ]]; then
  exit 1
fi

echo "claim drift ok"
