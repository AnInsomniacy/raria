#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/crates/raria-bt/tests/bt_gap_ledger.rs"
OUT="$ROOT/.omx/parity/generated/bt-gap-capability-index.yaml"

mkdir -p "$(dirname "$OUT")"

ruby - "$SRC" "$OUT" <<'RUBY'
require "yaml"
require "time"

src = ARGV[0]
out = ARGV[1]

mapping = {
  "BT-GAP-001" => ["bt.runtime.encryption", "raria_owned_subsystem", 7],
  "BT-GAP-002" => ["bt.runtime.webseed", "raria_owned_subsystem", 7],
  "BT-GAP-003" => ["bt.runtime.piece_strategy", "bt_session_authority", 7],
  "BT-GAP-004" => ["bt.runtime.mixed_source", "raria_owned_subsystem", 7],
}

entries = []
File.readlines(src, chomp: true).each_with_index do |line, idx|
  next unless line =~ /BT-GAP-\d+/
  gap_id = line[/BT-GAP-\d+/]
  capability_id, owner, phase_target = mapping.fetch(gap_id)
  entries << {
    "gap_id" => gap_id,
    "source_anchor" => "crates/raria-bt/tests/bt_gap_ledger.rs:#{idx + 1}",
    "capability_id" => capability_id,
    "owner" => owner,
    "phase_target" => phase_target,
  }
end

File.write(out, YAML.dump({"generated_at" => Time.now.utc.iso8601, "entries" => entries}))
RUBY

printf 'wrote %s\n' "$OUT"
