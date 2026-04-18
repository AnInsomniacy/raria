#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/.omx/parity/generated/claim-inventory.yaml"

mkdir -p "$(dirname "$OUT")"

ruby - "$ROOT" "$OUT" <<'RUBY'
require "yaml"
require "time"

root = ARGV[0]
out = ARGV[1]

files = [
  "README.md",
  "crates/raria-rpc/src/lib.rs",
  "crates/raria-rpc/src/methods.rs",
  "crates/raria-rpc/src/server.rs",
  "crates/raria-rpc/tests/ws_parity.rs",
  ".omx/plans/raria-aria2-parity-ralplan-draft-20260417.md",
]

patterns = [
  {id: "full_aria2_parity", regex: /full aria2 parity/i, needs_owner_resolution: false, needs_surface_classification: false, mapped_capability_ids: []},
  {id: "complete_aria2_compatible", regex: /complete aria2-compatible/i, needs_owner_resolution: false, needs_surface_classification: false, mapped_capability_ids: []},
  {id: "supports_all_aria2", regex: /supports all aria2/i, needs_owner_resolution: false, needs_surface_classification: false, mapped_capability_ids: []},
  {id: "aria2_onsourcefailed", regex: /aria2\.onSourceFailed/, needs_owner_resolution: false, needs_surface_classification: true, mapped_capability_ids: ["ws.notification.raria.onsourcefailed"]},
  {id: "aria2_onbtdownloadcomplete", regex: /aria2\.onBtDownloadComplete/, needs_owner_resolution: false, needs_surface_classification: true, mapped_capability_ids: ["ws.notification.aria2.onbtdownloadcomplete"]},
  {id: "system_listmethods", regex: /system\.listMethods/, needs_owner_resolution: false, needs_surface_classification: true, mapped_capability_ids: ["rpc.system.listmethods"]},
  {id: "system_listnotifications", regex: /system\.listNotifications/, needs_owner_resolution: false, needs_surface_classification: true, mapped_capability_ids: ["rpc.system.listnotifications"]},
]

claims = []

files.each do |rel|
  path = File.join(root, rel)
  next unless File.exist?(path)

  File.readlines(path, chomp: true).each_with_index do |line, idx|
    patterns.each do |pattern|
      next unless line.match?(pattern[:regex])

      claims << {
        "claim_id" => "#{pattern[:id]}:#{rel}:#{idx + 1}",
        "source_path" => rel,
        "source_line" => idx + 1,
        "claim_text" => line.strip,
        "claim_type" => "text_match",
        "mapped_capability_ids" => pattern[:mapped_capability_ids],
        "needs_owner_resolution" => pattern[:needs_owner_resolution],
        "needs_surface_classification" => pattern[:needs_surface_classification],
        "context_class" => "public_claim",
      }
    end
  end
end

payload = {
  "generated_at" => Time.now.utc.iso8601,
  "claims" => claims,
}

File.write(out, YAML.dump(payload))
RUBY

printf 'wrote %s\n' "$OUT"
