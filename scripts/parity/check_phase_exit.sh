#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <phase-id>" >&2
  exit 1
fi

PHASE_ID="$1"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

ruby - "$ROOT" "$PHASE_ID" <<'RUBY'
require "yaml"
require "json"

root = ARGV[0]
phase_id = Integer(ARGV[1])

policy = YAML.load_file(File.join(root, ".omx/parity/phase-exit-policy.yaml")).fetch("phases")
phase = policy.find { |entry| entry.fetch("phase_id") == phase_id }
abort("missing phase policy #{phase_id}") unless phase

phase.fetch("input_refs").each do |ref|
  path = File.join(root, ref)
  abort("missing phase input #{ref}") unless File.exist?(path)
end

def metric_value(root, metric, source_ref)
  path = File.join(root, source_ref)
  case metric
  when "claim_inventory_exists", "adr_exists", "capability_matrix_exists"
    File.exist?(path)
  when "blocked_claims_mapped"
    claims = YAML.load_file(path).fetch("claims")
    claims.all? { |entry| entry.key?("mapped_capability_ids") }
  when "bt_dht_key_owners_decided"
    decisions = YAML.load_file(path).fetch("decisions")
    required = %w[
      bt.runtime.metadata.persistence
      bt.runtime.peer.visibility
      bt.runtime.tracker.visibility
      bt.runtime.seeding.lifecycle
      dht.runtime.routing_table.persistence
      dht.runtime.bootstrap
    ]
    required.all? do |capability_id|
      decisions.any? { |entry| entry.fetch("capability_id") == capability_id && entry.fetch("resolution_state") == "decided" }
    end
  when "live_rpc_methods_generated", "live_rpc_notifications_generated"
    File.exist?(path) && !JSON.parse(File.read(path)).empty?
  when "parity_surfaces_not_ready"
    entries = YAML.load_file(path).fetch("entries")
    entries.none? do |entry|
      entry.fetch("policy_class") == "aria2_parity_surface" && entry.fetch("parity_claim_state") == "ready"
    end
  when "phase_exit_policy_complete"
    phases = YAML.load_file(path).fetch("phases")
    phases.map { |entry| entry.fetch("phase_id") }.sort == (0..8).to_a
  else
    abort("unsupported metric #{metric}")
  end
end

phase.fetch("required_assertions").each do |assertion|
  metric = assertion.fetch("metric")
  operator = assertion.fetch("operator")
  expected = assertion.fetch("value")
  actual = metric_value(root, metric, assertion.fetch("source_ref"))

  ok = case operator
       when "eq" then actual == expected
       when "all_true" then actual == true
       when "none_present" then actual == true
       else abort("unsupported operator #{operator}")
       end
  abort("phase #{phase_id} assertion failed: #{metric} #{operator} #{expected.inspect}; actual=#{actual.inspect}") unless ok
end

if phase.fetch("allow_start_next_phase")
  puts "phase #{phase_id} exit ok; next phase may start"
else
  puts "phase #{phase_id} exit ok; terminal gate remains closed as expected"
end
RUBY
