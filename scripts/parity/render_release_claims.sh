#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/.omx/parity/generated/release-claims.md"

mkdir -p "$(dirname "$OUT")"

ruby - "$ROOT" "$OUT" <<'RUBY'
require "yaml"
require "time"

root = ARGV[0]
out = ARGV[1]
entries = YAML.load_file(File.join(root, ".omx/parity/parity-ledger.yaml")).fetch("entries")

approved = entries.select { |entry| entry.fetch("approved_for_release") }

lines = []
lines << "# Release Claims"
lines << ""
lines << "Generated at #{Time.now.utc.iso8601}."
lines << ""
approved.each do |entry|
  lines << "- `#{entry.fetch("capability_id")}`: #{entry.fetch("raria_behavior_summary")}"
end

File.write(out, lines.join("\n") + "\n")
RUBY

printf 'wrote %s\n' "$OUT"
